# Android app (roadmap step 5): plan

Status: plan of 2026-11-06, checked by a second reviewer and revised; updated 2026-11-07. Built
on the trial's results ([android-trial.md](android-trial.md)) and the decisions at the end. Step 4
(voice tasks in the Rust core, [voice-tasks.md](voice-tasks.md)) was built at the same time; this
plan only uses its results.

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

- **No server.** The app talks to Soniox, the AI provider and Google itself,
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
  focus and window events (and key presses, only while a volume-down gesture is set). No alarms,
  no polling, no wake locks, no permanent foreground service. The only background job is one
  network-constrained retry for unsaved tasks.
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
| Bubble and gestures | Size, opacity, logo, "move it back to the start"; what tap, double tap and long press do; show on the lock screen or not; what the accessibility button, holding volume down and pressing it twice do. |
| AI revision | Provider, key, model, thinking, server address (local), how much to change, presets, your own rules, time limit, "try it". |
| Voice tasks | Google account (sign in or out), lists and their other names, the list for tasks that fit none, the AI that reads tasks (the revision one by default) and its time limit, event length and due date for timed tasks, how many tasks may wait on the lock screen, the tasks said there, waiting and saved lately (Edit, Undo), and "try it". |
| Listening | Stop after seconds of silence, time limit. |
| Typing | Persian letters, keep a copy in the clipboard. |
| Recent text | The last texts dictated, with the text before AI revision; copy, clear, how many to keep (on the phone only). |
| Usage | Soniox cost this month and today, minutes, dictations, and the balance left once it is entered; other services link to their own usage page. |
| General | Proxy, sounds and their volume, notifications, check for updates. |
| About | Version, check for updates by hand, copy the log, the project page. |

## Gestures and triggers

Defaults, all changeable: tap = dictate, double tap = dictate and revise, long press = voice task.
The first tap starts recording at once; a second tap within the double-tap time only marks the
dictation for revision (the two differ only after the audio), so a plain tap never waits. While
listening, a tap stops.

With no text field in front (home screen, a list), a tap opens the small task panel instead,
since there is nowhere to type: it shows what you say and the task it becomes. On the lock screen
every gesture that records makes a task for the queue.

Other ways to start (PR 4), set on the same screen:

- **Volume down**, held or pressed twice, each off by default. The typing service asks Android for
  key events (`canRequestFilterKeyEvents`, `FLAG_REQUEST_FILTER_KEY_EVENTS`) only while one of the
  two is set, and takes only volume down, only with the screen on and outside calls. A press is
  held back to see what it becomes: a plain press still turns the volume down (with "twice" set,
  a moment later, once no second press came), and holding it with nothing set for "held" turns it
  down step by step, both with Android's volume panel. While Mirza listens, one press stops it.
  The screenshot chord (power and volume down) is Android's and comes first: Android holds every
  volume-down press back 150 ms for it before Mirza sees the press (not the release), so the
  second press may come up to the double-tap time plus those 150 ms after the first release.
  Mirza turns the volume down with `adjustSuggestedStreamVolume`, the closest public call to what
  the key does; with a Cast session the key would change the remote volume, Mirza's press the
  phone's.
- **The accessibility button**, the floating one or the one in the navigation bar, dictating by
  default. Since Android 12 a service asks for it in its XML only, and Android then turns the
  button and the service on and off together (the service's page shows only a shortcut switch).
  Asking at runtime is ignored, and asking in the typing service's XML would put the button on
  everyone's screen and tie typing to it, both seen on the emulator. So the button lives in a
  second accessibility service, "Mirza button" (`ButtonService`), that reads nothing and gets no
  events: turn its shortcut on in Android's accessibility settings to get the button, off to
  lose it. It works only while Mirza's typing is on too.

No Quick Settings tile. Making Mirza the default digital assistant (long press on power) is left
out.

## Voice tasks on Android

Step 4 (PR erfnemati/mirza#10) builds the task code in the core (`crates/core/src/tasks/`): the
AI parser (one call through the same `[ai.*]` providers as revision, dates and Jalali in Rust),
the Google Tasks and Calendar calls, and the queue that keeps a transcript until Google confirms
the task. The phone uses it through `TaskBook` in `crates/mobile/src/tasks.rs`: `capture`,
`capture_held`, `confirm`, `process`, `process_due`, `retry_now`, `next_due_ms`, `waiting`,
`recent`, `lists`, `undo`, `edit`, plus `preview` ("try it") and `ai_problem`. `MirzaCore.task_book`
makes it from the phone's `[tasks]` settings (the desktop's task settings without the Google
client, plus `held_limit`) and keys, and `update_task_book` hands it changed settings or keys.
Calls that reach Google or the AI block, so Kotlin runs them off the main thread; `capture`,
`capture_held` and `confirm` are fast. The notification's title and text come from the core
(`TaskOutcome`), and each task comes with its list name and its day in words ("tomorrow 15:00");
showing them and the buttons is Kotlin's job. Android adds:

- **Google sign-in through Google Play services** (`AuthorizationClient`): one tap, no secret in the app, Google's way for Android. It asks for exactly the
  scopes the desktop uses: `https://www.googleapis.com/auth/tasks` and
  `https://www.googleapis.com/auth/calendar.events.owned`. Sign-in and every token refresh use
  the phone's own network, not Mirza's proxy, so the phone's VPN has to reach Google at that
  moment; the Tasks and Calendar calls themselves go through the proxy setting.
  Needs Play services (global Xiaomi phones have it). There is no refresh token on the phone: the
  core calls Kotlin's `GoogleTokens.access_token(fresh)` before every Google call, retries
  included, and Kotlin answers with `authorize()` (silent once allowed). `fresh = true` means
  Google just refused the last token, so Kotlin clears it first and asks for a new one. The answer
  is a `GoogleToken`: the token, `SignedOut` (the task is kept until the user signs in) or
  `Offline` (Google couldn't be reached, so the task waits and tries again). It is an enum rather
  than an error because UniFFI can't carry a flat error from Kotlin back to Rust. The
  Google Cloud project is the desktop's, with an Android OAuth client for `dev.erfnemati.mirza`
  and the SHA-1 under "Signing key" (step 7 of `docs/voice-tasks.md`). While the project is in
  "Testing", only its listed test users can sign in and a sign-in ends after 7 days; "In
  production" without verification allows up to 100 users with a warning screen once.
- **The capture flow:** gesture, microphone service, Soniox. The task panel (a card over the app
  in front) shows what is heard, with Done and Cancel. At the end `capture` keeps the text on the
  phone and `process` reads and saves it; the panel then shows "Added: <title>" with the list and
  the day, and Undo, Edit and Done, and goes away by itself after a few seconds. If the panel was
  left before, a notification with Undo and Edit says it instead, and so does one for a task that
  waits or is stuck. Edit opens the panel's form. Events use the calendar's default reminders;
  Mirza has no alarm or reminder of its own. If even keeping the text fails, it goes to the
  clipboard.
- **One queue.** The lock-screen queue is the core's own queue with a hold flag: while the phone
  is locked (`KeyguardManager.isKeyguardLocked`), Kotlin calls `capture_held(text)`, which stores
  only the transcript and sends nothing to the AI or Google. After unlock, one notification says
  how many tasks are held; tapping it goes through them one by one, a screen for each with
  Confirm (`confirm(id)` then `process(id)`), Edit (`edit`, which also releases it) and Drop
  (`undo`). There is no "Confirm all". Times are read from the
  moment the task was said, so "tomorrow" stays the day after it was said even if it is
  confirmed later; each screen shows when it was said, and after Confirm the task as read, with
  its day. Recognition needs the internet, so with no connection nothing is recorded.
  `[tasks] held_limit` (1 to 10) caps the queue; when it is full `capture_held` refuses with a
  plain message, which the "tasks wait" notification shows, and the error sound plays (toasts
  don't show on many lock screens).
- **One retry job:** a WorkManager job with a network constraint, first run after
  `next_due_ms`, that gets a token and calls `process_due` (held tasks are skipped).

## Order of work (one PR each into develop)

1. **Skeleton and dictation.** Compose check, the release signing key and signed builds, new app
   id, setup checklist, config and keys through the core, Speech and General screens, update check
   (opens the release page). Dictation from the trial, cleaned up: accessibility bubble only, mode
   B, the trial's typing fixes (hint text, no cursor, password fields), audio kept while
   connecting, notifications only while recording.
2. **The other screens and AI revision.** Bubble and gestures, AI revision ("dictate and revise"
   through the core), Listening, Typing, History and usage. The launcher icon and the bubble's
   logo become the desktop's (`assets/icons/io.github.erfnemati.Mirza.svg`, the teal rounded
   square with the white microphone).
3. **Voice tasks.** Sign-in, the task panel and the Voice tasks screen, the capture flow, Undo and
   Edit, the lock-screen queue and the retry job.
4. **Triggers and polish:** accessibility button (its own small service, see "Gestures and
   triggers"), volume key, animations (bubble colors, listening pulse, press and appear, screens
   that slide, the task panel sliding up and fading between its views), and a battery check over a
   day of use on the phone.
5. **Release:** the Release workflow builds and signs the APK and attaches it to the GitHub
   release next to the desktop installers (only the real app, not the trial), and an install
   guide (Play Protect, restricted settings, Xiaomi).

Each PR is tested on the KVM emulator with the virtual microphone (Persian clips through live
Soniox) and says what was tested there. What the emulator can't cover goes into one phone
checklist (below), run once on the release candidate instead of once per PR.

## Building and trying it

From `android/`: `./gradlew :app:assembleDebug` gives `app/build/outputs/apk/debug/app-debug.apk`
(add `-Pmirza.abis=x86_64` for the emulator only, which builds faster). Debug builds are signed
with the release key when it is found, so they install over a release build and the other way
around; they also carry `DebugReceiver`, adb hooks that only the shell can use:

    adb shell am broadcast -n dev.erfnemati.mirza/.DebugReceiver -a mirza.TAP      # tap the bubble
    ... -a mirza.TAP --es mode revise                                               # the same, revised by AI
    ... -a mirza.SWITCH --es mode revise                                            # a double tap while it records
    ... -a mirza.TAP --es mode task                                                 # a voice task, with the panel
    ... -a mirza.TASK --es text_b64 <base64 text> [--ez held true]                  # a task without the microphone
    ... -a mirza.SET --es path proxy --es json '"http://10.0.2.2:10808"'           # change a setting
    ... -a mirza.UPDATE                                                             # check for updates

On the emulator, a proxy on the host at `127.0.0.1:10808` is `http://10.0.2.2:10808`.
A key can go in without showing up in any command line, fed through stdin:
`adb exec-in "run-as dev.erfnemati.mirza sh -c 'mkdir -p files/mirza && cat > files/mirza/keys.toml'" < keys.toml`,
then `adb shell "run-as dev.erfnemati.mirza chmod 700 files/mirza"` and `chmod 600` on the file
the same way (`exec-in` ends the shell when the input ends, so nothing after `cat` runs there).
Remove the key after the test.

`uiautomator dump` pauses the typing service while it runs, so screen tests tap by coordinates
from screenshots instead. `adb shell input keyevent` never reaches an accessibility key filter
(injected keys skip it), so volume-down tests need a real key: `adb root`, then
`adb shell "sendevent /dev/input/event1 1 114 1; sendevent /dev/input/event1 0 0 0"` for down and
the same with `1 114 0` for up (event1 is the emulator's keyboard; `getevent -pl` lists them).
`adb unroot` afterwards. If the emulator's microphone gives only silence ("microphone silent
after 3 s") even after `emulator.sh mic`, restarting the emulator fixes it.

## Phone checklist

Run once on the release candidate APK, on a real phone, installed the way a user would (from the
browser or the file manager, not adb). Each PR adds what the emulator couldn't cover.

From PR 1 (skeleton and dictation):

- [ ] Install; Play Protect may block it (see the install guide). Mirza installs next to the
      trial as a separate app.
- [ ] Open Mirza. Allow the microphone and notifications from the setup list.
- [ ] Typing: open Accessibility from the list and turn Mirza on. If Android says the setting is
      restricted: App info, the menu at the top, Allow restricted settings, then again. Back in
      Mirza the status says Ready and the bubble shows.
- [ ] If the trial is installed: turn off its typing (the list asks for it while it is on).
- [ ] Enter the Soniox key on the Speech screen; set the proxy on the General screen if needed.
- [ ] Xiaomi: the Autostart and Battery saver buttons open the right pages; set them and tick the
      steps.
- [ ] Dictate Persian and English mixed in Telegram, WhatsApp, a browser field and one more app.
      The text lands at the cursor, half-spaces are right, nothing is lost at the start.
- [ ] While recording, "Mirza is listening" with a Stop button; Stop ends it and the
      notification goes away. No Mirza notification when idle.
- [ ] Long press on the bubble does what Bubble and gestures says; drag moves it and the place
      stays.
- [ ] Mirza leaves recent apps once you leave it, and typing stays on. Restart the phone: the
      bubble comes back by itself.
- [ ] How it feels: start delay, sounds.

From PR 2 (the other screens and AI revision):

- [ ] The launcher icon is the desktop logo, also with the launcher's themed icons on; every
      screen reads well in dark mode.
- [ ] AI revision with a real key: double tap the bubble and say something with fillers and a
      correction ("no wait, ..."). The bubble turns purple while revising and the revised text
      lands; Recent text shows "Before AI revision". A tap while it is purple types the text
      without revision.
- [ ] "On the lock screen" off: the bubble is gone on the lock screen and back right after
      unlocking with the PIN and with the fingerprint.
- [ ] The volume slider plays a test sound at the chosen volume; start and stop sounds follow it.
- [ ] Usage shows the Soniox numbers (through the proxy if one is set).

From PR 3 (voice tasks):

- [ ] Voice tasks: Sign in picks the Google account and asks for Tasks and Calendar; the screen
      then says signed in and Read again shows the lists. Give a list other names.
- [ ] Long press the bubble in any app and say a task with a project and a time ("tomorrow at 3,
      meeting about the contract for <project>"). The panel shows the words, then "Added" with the
      right list and day; the task is in Google Tasks and the event in Google Calendar, with the
      calendar's reminder. Undo removes both; Edit changes them.
- [ ] Say a task and press Home before it is saved: a notification "Added: ..." comes instead,
      with working Undo and Edit buttons.
- [ ] Tap the bubble on the home screen: the panel opens, and what you say becomes a task.
- [ ] Lock the phone and say two tasks from the bubble there (nothing is shown, the sounds say
      it). Unlock with the PIN or fingerprint: "2 tasks wait for you". Tap it: confirm one, drop
      the other. A task with "tomorrow" said before midnight and confirmed after keeps its day.
- [ ] Sign out and say a task: it is kept ("Task kept, not saved yet") and listed as waiting.
      Sign in again: it is saved by itself.

From PR 4 (triggers and polish):

- [ ] Bubble and gestures: set "Hold volume down" to a voice task and "Press volume down twice" to
      dictate. In a text field, press twice: dictation starts; one press stops it. Hold: the task
      panel opens. A single press still lowers the volume, a moment later; holding with "held" set
      to Nothing lowers it step by step.
- [ ] Power and volume down together still take a screenshot. During a call, and while the phone
      rings, volume down does what it always does. With the screen off it does nothing for Mirza.
- [ ] "Accessibility settings" on that screen, turn on the "Mirza button" shortcut: the
      accessibility button shows (floating, or in the navigation bar with three-button
      navigation) and starts dictation. Turn the shortcut off: the button goes, typing stays on.
      On Xiaomi, check the button works and that Mirza button stays on after a restart.
- [ ] The animations feel right on the phone: screens slide, the task panel slides up and fades
      between its views, the bubble pulses while listening and shrinks a little under the finger.
      With animations off in Developer options, nothing pulses.
- [ ] Battery: a day of normal use with the bubble on and a volume gesture set. Settings, Battery,
      Mirza should be about as low as before PR 4 (note the number).

## Signing key

Made on 2026-11-06: RSA 4096, PKCS12, alias `mirza`, valid until 2054. It lives in `.secrets/` at
the root of the main clone (`android-release.p12` and `android-release.password`, mode 600),
ignored by git and listed in `.backup-paths`, so the backup copies it. The Release workflow gets
it from the GitHub secrets `MIRZA_ANDROID_KEYSTORE` (the file in base64) and
`MIRZA_ANDROID_KEYSTORE_PASSWORD`; GitHub can't show them again, so the backup is the only other
copy. If the key is lost, every user has to uninstall Mirza and install it again.

Certificate fingerprints (public; Google Cloud asks for the SHA-1 when the Android OAuth client is
created for `dev.erfnemati.mirza`):

- SHA-1: `B2:81:EB:62:D0:7D:2C:89:2C:FD:7A:9B:68:76:AE:CA:74:9E:48:ED`
- SHA-256: `FF:AC:89:7A:EA:E6:59:23:83:9D:79:B6:EE:F0:A1:DE:6D:FE:5E:8B:F6:A0:2A:D1:DA:2C:2C:61:D4:22:8C:42`

## Decisions

- No central server; the signing key as above; Google sign-in through Play services (2026-11-06).
- A full lock-screen queue refuses new tasks with a plain message; after unlocking, the held tasks
  are confirmed one by one, with no "Confirm all" (2026-11-07).
- Only the real app ships in releases; the trial stays a development tool (2026-11-07).

## Risks

- HyperOS may still stop the accessibility service ("clear all", the phone's own cleaner); the
  setup checklist and the "typing is off" screen are the answer, plus a keyboard (IME) later if
  needed.
- Play Protect blocks every sideloaded build with an accessibility service; the install guide has
  to cover it, and it may get stricter.
- Google may tighten unverified apps or the Tasks and Calendar scopes; then the project needs
  verification.
- Compose adds about 1 to 2 MB to the APK after R8, and a longer first build.
