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
