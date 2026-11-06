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
4. **Voice tasks.** A "capture task" action: speech, then AI, then a typed task (title, notes,
   list, tags, date, time, priority, repeat) checked against a schema, saved to Google Tasks. The
   Google Tasks API keeps only the date, so list = category and tags/priority go in the notes.
   No confirm step: a notification with Undo and Edit. Transcripts wait in a local queue until
   the task is saved, so nothing is lost offline. Dates resolve against when they were said;
   Jalali dates are converted in Rust.
5. **Android app.** Kotlin app over the Rust core: floating bubble (size, logo, opacity), gestures
   (tap, double tap, long press) mapped to actions, dictation, AI revision and voice tasks. No
   alarm of its own: a task with a time also becomes a Google Calendar event, whose reminder
   rings. Plan: [android-app.md](android-app.md).
6. **Microsoft To Do** (full reminders, categories and priority through Microsoft Graph).
7. **iPhone**: a custom keyboard and a Shortcuts action over the same Rust core.
