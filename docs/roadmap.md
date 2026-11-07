# Roadmap

Agreed in November 2026. Each step is one or more PRs into `develop`.

1. **Soniox v6 (0.1.6).** Default model `stt-rt-v6`; send the key in the `Authorization` header
   instead of the start message. Soniox refuses keys sent only in the start message from
   2027-01-15, so 0.1.5 stops working then. v5 is removed on 2026-12-15 and routed to v6.
   Later, after tests with real Persian clips: v6 writes spoken commands ("period", "new line",
   "scratch that") as words for the app to apply.
2. **Android trial.** Build `mirza-core` for Android (cargo-ndk, UniFFI) and test on a real phone
   the parts that decide the design: overlay bubble, text insertion through an
   AccessibilityService, and the microphone foreground service (Android 14+ can't start it from
   the background).
3. **AI revision (optional, off by default).** Clean the transcript (fillers, repeats, last
   self-correction wins, punctuation, half-spaces, both languages kept) with OpenAI, Gemini,
   Claude, OpenRouter or a local OpenAI-compatible server. A separate "dictate and revise" action;
   if the AI fails or times out, the raw text is typed.
4. **Voice tasks.** Its own shortcut (and its own bubble gesture on the phone): speech, then one
   AI call through the `[ai.*]` providers with a JSON schema checked in Rust, then a task in
   Google Tasks: a title of four or five words, a one-line note only for extra details, and the
   list that fits (one list per project or area, with other names set in the settings; no tags).
   Google Tasks keeps no time, so a task with a clock time also becomes an event in the main
   Google Calendar, whose reminder rings. No confirm step: a notification with Undo and Edit.
   Spoken tasks wait in a queue on disk until Google confirms them, so nothing is lost offline.
   Dates resolve against when they were said; Jalali dates are converted in Rust. Without AI, or
   when it fails, the words are saved as said. Guide: [voice-tasks.md](voice-tasks.md).
5. **Android app.** Kotlin app over the Rust core: floating bubble (size, logo, opacity), gestures
   (tap, double tap, long press) mapped to actions, dictation, AI revision and voice tasks. No
   alarm of its own: a task with a time also becomes a Google Calendar event, whose reminder
   rings. Plan: [android-app.md](android-app.md); install guide:
   [install-android.md](install-android.md).
6. **Microsoft To Do** (full reminders, categories and priority through Microsoft Graph).
7. **iPhone**: a custom keyboard and a Shortcuts action over the same Rust core.
