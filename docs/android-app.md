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
  `usage::http_client`), as on the desktop. The one exception is Google sign-in if it goes through
  Google Play services (see "Decisions to ask"): that uses the phone's own network.
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
  the same release key (decision 2 below), so updates install over each other and Google sign-in
  sees one certificate. The trial (`dev.erfnemati.mirza.trial`) stays until the real app covers its
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

Step 4 builds the task code in the core: the AI parser, the Google Tasks and Calendar calls, and
the queue that keeps a transcript until the task is saved. Android adds:

- **Google sign-in**, the way Erfan picks (decision 1). The core already accepts either a refresh
  token (its own sign-in) or an access token handed over by Kotlin. With Play services the access
  token lasts about an hour, so Kotlin fetches a fresh one (`authorize()`, silent once allowed)
  before every core call, retries included.
- **The capture flow:** gesture, microphone service, Soniox, then the core's task call, then a
  notification "Added: <title> (<list>)" with Undo and Edit. Edit opens the task panel.
- **One queue.** The lock-screen queue is the core's own queue with a "needs confirmation" flag:
  while the phone is locked (`KeyguardManager.isKeyguardLocked`), Kotlin stores the transcript
  with that flag and sends nothing to the AI or Google. After unlock, one notification lists the
  queued tasks; tapping it opens the panel to confirm or drop them. Recognition needs the
  internet, so with no connection nothing is recorded. A full queue refuses new tasks with a
  message (to confirm with Erfan). Asked of step 4: the flag on its queue.
- **One retry job:** a WorkManager job with a network constraint that gets a token and calls the
  core to save what is waiting (not the tasks still needing confirmation).
- Sign-in, the Tasks screen and the task panel are Kotlin and can be built against a stub before
  step 4 is merged; only the real task call waits for it.

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
Soniox), then on Erfan's phone with a short checklist; the PR says which is which. PR 1's phone
checklist includes recording from the lock screen (a microphone service started from the lock
screen on Android 14+ can only be checked on the emulator).

## Decisions to ask Erfan (one at a time)

1. **Google sign-in.** (a) Google Play services sign-in (`AuthorizationClient`): Google's way for
   Android, one tap, no secret in the app; but sign-in and every token refresh use the phone's own
   network, not Mirza's proxy, so the phone's VPN must reach Google then. Needs Play services,
   which global Xiaomi phones have. (b) Browser sign-in in a Custom Tab with Mirza keeping a
   refresh token (as on the desktop): everything goes through Mirza's proxy, but Google turned
   this off by default for new Android clients and asks apps to use (a); it has to be switched on
   again in the client's advanced settings. Recommended: (a). Either way Erfan creates the Google
   Cloud project and names it (step 4 already asks him for it), published "In production" without
   verification: up to 100 users and a warning screen once ("Testing" would end the sign-in every
   7 days).
2. **The signing key**, needed before PR 1's first build for the phone. Recommended: one release
   key made on this server, kept out of git, listed in `.backup-paths` so his laptop copies it, and
   added as a GitHub secret for the Release workflow. If it is lost, everyone has to uninstall and
   install again, with the Play Protect and restricted-settings steps.
3. Smaller ones, when PR 3 starts: a full lock-screen queue refuses new tasks with a message?
   Confirm queued tasks one by one or all at once?

## Risks

- HyperOS may still stop the accessibility service ("clear all", the phone's own cleaner); the
  setup checklist and the "typing is off" screen are the answer, plus a keyboard (IME) later if
  needed.
- Play Protect blocks every sideloaded build with an accessibility service; the install guide has
  to cover it, and it may get stricter.
- Google may tighten unverified apps or the Tasks and Calendar scopes; then the project needs
  verification.
- Compose adds about 1 to 2 MB to the APK after R8, and a longer first build on the server.
