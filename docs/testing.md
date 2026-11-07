# Testing a Mirza build

Thanks for trying Mirza. Use a spare minute of audio credit, and tell us what
went wrong (a screenshot helps) and your system (e.g. "Windows 11, Persian
keyboard layout").

## Setup

- [ ] **Install.** Follow the install steps for your system in the README.
      Note anything confusing.
- [ ] **Tray icon.** A microphone icon shows up in the tray / menu bar.
- [ ] **Settings window.** Clicking the icon opens it (on macOS, the menu has
      **Settings…**).
- [ ] **API key.** Paste a Soniox key and save it; the notice asking for a key
      goes away.
- [ ] **Shortcut.** Add a shortcut and press your keys; they show up in the row.

## Dictation

- [ ] **Toggle mode.** Open a text editor, press the shortcut, say *"Hello,
      this is a test."*, press it again. The text is typed.
- [ ] **Mixed Persian and English.** Say *«سلام، این یک آزمایش است. Please
      open the file.»*. Check:
  - both languages come out right;
  - the half-space (نیم‌فاصله) is right;
  - the words are in the right order.
- [ ] **Hold mode.** Set a hold-to-talk shortcut. Hold it, speak, let go. The
      text is typed after you let go.
- [ ] **Single key** (Windows/macOS). Set Right Ctrl (Windows) or Right Option
      (macOS) as a hold key and check:
  - holding it dictates;
  - Right Ctrl+C still copies, and doesn't start dictation.
- [ ] **Browsers and chat apps.** Dictate into a browser text box and into a
      chat app.
- [ ] **Switching windows.** While dictating, switch to another window. Typing
      pauses, and the rest ends up in the clipboard.
- [ ] **Silence.** Stay silent for a minute. Mirza stops by itself.

## AI revision

Needs a key for one AI provider (see [ai-revision.md](ai-revision.md)), or a
local server.

- [ ] **Setup.** In Settings > AI revision, pick the provider and paste the
      key. **Try it** with *«خب من فردا نه ببخشید پس فردا می خوام برم
      دانشگاه»*: you get «من پس‌فردا می‌خوام برم دانشگاه.» or close to it.
- [ ] **Shortcut.** Set **Dictate and revise**. Say a sentence with an "um"
      and a self-correction. The cleaned text is typed; Recent text shows
      both versions.
- [ ] **Mixed languages.** Say *«این PR رو review کن و بعد merge کن»*. The
      English words stay English and nothing is translated.
- [ ] **Polish.** Choose **Full polish** and turn on **Plain words** and
      **Numbers as digits**. Say a long, rambling sentence with a price in it
      («قیمتش صد و پنجاه هزار تومنه»). It comes back shorter and tidier, with
      every point kept and the price as «۱۵۰ هزار».
- [ ] **Failure.** Paste a wrong key for the AI provider, then dictate and
      revise. What you said is typed unrevised, with a notification saying
      why.
- [ ] **Second press.** While it says "Revising with AI…", press the shortcut
      again. The unrevised text is typed right away.

## Voice tasks

Needs a Google client of your own and an AI key (see
[voice-tasks.md](voice-tasks.md)). Run it on Linux, Windows and macOS. Delete
the test tasks and events afterwards.

- [ ] **Sign in from the settings window.** In Settings > Voice tasks, paste
      the client file and save it, then click **Sign in**. The browser opens;
      tick both boxes. The window shows your lists. Try **Cancel** once while
      it waits.
- [ ] **Sign in from a terminal.** Sign out, then run `mirza google-login`.
      It says it's signed in, and the settings window agrees.
- [ ] **Task shortcut.** Set **Say a task**, then say *"call Sara tomorrow at
      3"*. A notification shows the title, the list, "tomorrow 15:00" and "in
      Calendar too". Google Tasks has the task and Google Calendar the event.
- [ ] **Hold to say a task.** The same with **Hold to say a task**.
- [ ] **Persian.** Say *«بیستم آذر ساعت ۱۰ صبح جلسه با تیم»*. The date is
      right in Google Calendar.
- [ ] **Undo.** Use the notification's **Undo** (Linux and Windows; on macOS,
      or if your desktop shows no buttons, use **Undo task** in the tray
      menu). The task and the event are gone from Google.
- [ ] **Edit.** Use **Edit** in the notification or the tray menu: the
      settings window opens on the task. Change the time and the list and
      save; Google follows. Do it once with the settings window already open.
- [ ] **Offline.** Turn off the network, say a task: "Task kept, not saved
      yet". Turn it back on: it's saved by itself within a few minutes.

## Other checks

- [ ] **Usage.** The tray menu shows this month's Soniox spend.
- [ ] **Switching providers.** Switch provider from the tray menu; the
      settings window follows.
- [ ] **Start on login.** Restart the computer; Mirza starts on its own.
- [ ] **Windows: admin windows.** Dictate into a program run as administrator.
      You get a message, and the text is in the clipboard.
- [ ] **macOS: permissions after an update.** Install a newer build over the
      old one. Check whether macOS asks for the permissions again.

## Where to look when something breaks

Run `mirza` from a terminal to see its log. Add `MIRZA_LOG=mirza=debug` for more
detail; this shows the recognized text.
