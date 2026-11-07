# Voice tasks

Press the task shortcut, say a task, and Mirza saves it to Google Tasks. If you said a clock
time, it also puts an event in your Google Calendar, so Google reminds you on every device.

"Call Sara about the contract tomorrow at 3" becomes a task "Call Sara about the contract" due
tomorrow, and a 30-minute event tomorrow at 15:00. «یادم بنداز پنج‌شنبه کتاب رو به علی پس بدم»
becomes a task «کتاب رو به علی پس بدم» on Thursday, with no event because no time was said.

There is no confirm step. Mirza saves the task at once, then shows a notification with the
title, the list, the date and time, and anything it had to guess, with **Undo** and **Edit**.

## Setting up Google

Mirza signs in to your Google account to add tasks and events. Google requires every app that
does this to have its own "OAuth client" in a Google Cloud project. Until Mirza ships with one,
you create it once, for free, in your own Google account. It takes about ten minutes. You need
no credit card.

If someone already gave you a client ID for Mirza, skip to [Signing in](#signing-in).

### 1. Create a project

1. Open [console.cloud.google.com](https://console.cloud.google.com/) and sign in with your
   Google account. Accept the terms if it asks.
2. Click the project picker at the top left (it says **Select a project**, or the name of a
   project you already have), then **New project**.
3. **Project name**: `Mirza`. Leave **Location** as it is (**No organization** for a personal
   account). Click **Create**.
4. When the notification says the project is ready, click **Select project**. The picker at the
   top should now say Mirza. Check this before each of the next steps.

### 2. Turn on the Tasks and Calendar APIs

1. Open [APIs & Services > Library](https://console.cloud.google.com/apis/library).
2. Search for **Google Tasks API**, open it and click **Enable**.
3. Go back to the library, search for **Google Calendar API**, open it and click **Enable**.

### 3. Describe the app (consent screen)

1. Open [Google Auth Platform](https://console.cloud.google.com/auth/overview) and click **Get
   started**.
2. **App information**: App name `Mirza`; User support email: your address. **Next**.
3. **Audience**: choose **External**. **Next**. (Internal only exists for Google Workspace
   organizations.)
4. **Contact information**: your address. **Next**.
5. Tick that you agree to the Google API Services User Data Policy, then **Continue** and
   **Create**.

Don't upload a logo. A logo makes Google want to verify the app first, and you don't need
verification for yourself and a few friends.

### 4. Choose what Mirza may do (scopes)

1. In Google Auth Platform, open [Data Access](https://console.cloud.google.com/auth/scopes)
   and click **Add or remove scopes**.
2. At the bottom, under **Manually add scopes**, paste these two lines and click **Add to
   table**:

   ```
   https://www.googleapis.com/auth/tasks
   https://www.googleapis.com/auth/calendar.events.owned
   ```

3. Check that both are ticked in the table, then click **Update** and **Save**.

The first lets Mirza see, add, change and delete tasks. The second lets it see, add, change and
delete events in calendars you own, and nothing else: it can't see calendars others share with
you or change any calendar's settings. Mirza reads your list names, and changes only the tasks
and events it added itself.

### 5. Publish the app

1. Open [Audience](https://console.cloud.google.com/auth/audience).
2. Under **Publishing status** it says **Testing**. Click **Publish app**, then **Confirm**.
3. It now says **In production**. If Google mentions verification, ignore it: don't click
   **Prepare for verification**.

If **Publish app** is greyed out, Google wants the
[Branding](https://console.cloud.google.com/auth/branding) page complete first: open it, fill in
what it marks as missing (still no logo), save, and come back.

Why: in Testing, Google signs Mirza out every 7 days, and only the accounts listed under **Test
users** on the Audience page can sign in at all. In production without verification, the sign-in
lasts, and up to 100 Google accounts can ever use your client, which is plenty for you and a few
friends. The price is a warning screen at sign-in (see below).

### 6. Create the client

1. Open [Clients](https://console.cloud.google.com/auth/clients) and click **Create client**.
2. **Application type**: **Desktop app**. **Name**: `Mirza desktop`. Click **Create**.
3. A window shows the client ID and the client secret. Click **Download JSON** now: Google
   shows the secret only this once. Keep the file somewhere private. (If you lose it, open the
   client, add a new secret and download that.)

Mirza needs both. For a desktop app the "secret" isn't really secret (Google says so itself:
anyone can pull it out of the program), but there's no reason to post it.

### 7. The phone's client (Mirza for Android)

Mirza on Android signs in through Google Play services and needs a client of its own in the same
project. It has no secret and no file to download. Create it once, even before the phone app is
out:

1. In [Clients](https://console.cloud.google.com/auth/clients), click **Create client**.
2. **Application type**: **Android**. **Name**: `Mirza Android`.
3. **Package name**: `dev.erfnemati.mirza`.
4. **SHA-1 certificate fingerprint**: `B2:81:EB:62:D0:7D:2C:89:2C:FD:7A:9B:68:76:AE:CA:74:9E:48:ED`
   (the key Mirza's Android releases are signed with; see
   [android-app.md](android-app.md#signing-key)). Click **Create**.

The phone uses the same scopes and the same publishing status as the desktop.

### Signing in

1. Open Mirza's settings and go to **Voice tasks**. Open the downloaded JSON file in a text
   editor, copy all of it, paste it under **Google client** and click **Save**.
2. Click **Sign in**. Your browser opens Google's sign-in page.
3. Google says **Google hasn't verified this app**. Click **Advanced**, then **Go to Mirza
   (unsafe)**. This is your own app, so that's expected.
4. Google lists what Mirza asks for. **Tick both boxes** (Tasks and Calendar) and click
   **Continue**. Without the calendar box, tasks with a time are saved as tasks only.
5. The browser says you can close the page. The settings window shows your lists.

From a terminal, `mirza google-login --client ~/Downloads/client_secret_….json` saves the client
and signs in; later sign-ins need only `mirza google-login`.

On a computer without a browser (or if the browser doesn't open), the settings window and the
terminal both show the sign-in address. Open it on another device and sign in there. The browser
then ends up at an address that starts with `http://127.0.0.1` and shows an error, which is
fine: copy that address and paste it into the settings window (under **Waiting for Google**) or
into the terminal.

Mirza keeps the sign-in (a "refresh token") in the same file as your API keys, readable only by
you. **Sign out** in the settings (or `mirza google-logout`) removes it and tells Google to
cancel it. You can also remove Mirza's access at
[myaccount.google.com/connections](https://myaccount.google.com/connections).

If you don't use voice tasks for six months, Google ends the sign-in by itself; sign in again.

## Saying a task

Set a shortcut in **Settings > Shortcuts**: **Say a task** (press, speak, press again) or **Hold
to say a task**. They are separate from the dictation shortcuts, and nothing is typed into the
window you're in. If your desktop runs the shortcuts itself, bind the command `mirza toggle-task`.

While you speak, a notification says "Say a task…". When you stop, Mirza first writes what you
said to disk, then reads it with AI and saves it to Google. `mirza add-task call Sara tomorrow
at 3` does the same with typed text.

The AI turns what you said into:

- a **title** of four or five words, in your own words and language, without the date or time;
- a **note**, only when you gave extra details;
- a **list**: the Google Tasks list whose name, or one of its other names (see
  [Lists](#lists)), fits what you said;
- a **day** and a **time**, and how long it takes if you said so.

Days like "tomorrow", "next Monday" or «پنج‌شنبه» count from when you said the task, not from
when it reached Google. Jalali dates («۱۵ آذر») are converted to Gregorian by Mirza itself, not
by the AI. When something could be read two ways (3 o'clock in the morning or the afternoon,
which week a weekday is in), Mirza saves its best guess and the notification says what it
guessed, so you can fix it with Edit.

Without AI, or when the AI fails or doesn't answer within the time limit, the task is saved as
you said it: the first five words as the title, the whole text as the note, in the default list,
with no day or time. The notification says so.

### Tasks with a time

Google Tasks keeps only the day of a task, never the time, and its reminders don't ring at a
time. So when you say a clock time, Mirza saves the task and also adds an event to your main
Google Calendar at that time, 30 minutes long unless you said otherwise. The event uses your
calendar's default reminders, so you get the same reminder you get for any other event. The time
zone is the one your computer was in when you said the task.

By default the task is also due that day in Google Tasks. Turn off **Due date for timed tasks**
to keep the day only on the event.

### Lists

Mirza reads your lists from Google Tasks and picks the one that fits. In **Settings > Voice
tasks > Lists** you can give each list other names, such as a client, a project or a place
("office, Acme, the Berlin job"), so Mirza knows those tasks belong there. Tasks that fit no
list go to the list under **When nothing fits** (your Google default list unless you choose
another), and the notification says so. Mirza reads the lists again every ten minutes at most;
**Read again** does it now, after you add a list in Google.

## Undo and Edit

After each task a notification shows the title, then the list, the day and time ("in Calendar
too" when there's an event), and anything Mirza guessed.

- **Linux**: the notification has **Undo** and **Edit** buttons, if your desktop's notifications
  show buttons (KDE and GNOME do).
- **Windows**: the buttons work while the notification is on screen.
- **macOS**: notifications have no buttons here.

On every system, the tray menu has **Undo task** and **Edit task** for the last task, and
**Settings > Voice tasks > Saved lately** lists the last 20 with both buttons.

**Undo** deletes the task, and its event, from Google. **Edit** opens the settings window on a
form with the title, note, list, day, time and length. Saving it changes the task in Google:
moving it to another list, adding an event when you add a time, or deleting the event when you
remove the time.

## When Google can't be reached

Nothing you say is lost. Every task is written to disk before anything is sent, and stays there
until Google confirms it has it. If Google, the AI or the internet is down, Mirza tries again by
itself after 30 seconds, then 1, 2, 5 and 10 minutes, then every 15 minutes. If an answer from
Google gets lost on the way, Mirza looks for the task in Google before sending it again, so you
don't get it twice.

Some problems need you first: not signed in, an API turned off in your Google Cloud project, or
an unticked box at sign-in. Then the notification says what to do, the task waits, and Mirza
tries again every 15 minutes and right after you sign in. **Settings > Voice tasks** lists the
waiting tasks with what went wrong; **Try now** sends them at once, and you can edit or delete
them there too.

The waiting tasks are in Mirza's data folder, under `tasks/queue`: `~/.local/share/mirza` on
Linux, `%LOCALAPPDATA%\mirza` on Windows and `~/Library/Application Support/mirza` on macOS.

## Settings

Everything on the Voice tasks page is also in the settings file, under `[tasks]`:

```toml
[tasks]
provider = ""            # AI for tasks; "" uses the one from AI revision
timeout_sec = 30         # then the task is saved as said
google_client_id = ""    # set by pasting the client file; the secret is kept with the API keys
default_list = ""        # list id for tasks that fit no list; "" is your Google default list
event_minutes = 30       # event length when you don't say one
timed_due_date = true    # a task with a time is also due that day

[tasks.list_hints]       # other names for a list, by list id
"MDk3NTE3NjQ5NjQ5NjQ4NjU4MzQ6MDow" = "office, Acme"
```

The AI uses the model set for that provider under **AI revision**. A fast, cheap model is
enough: `claude-haiku-5-5` read most of our 25 test tasks right, in one to six seconds. **Try it** on
the Voice tasks page shows how a sentence would be read, without saving anything.

## Privacy

- Your voice goes to your speech service, as with dictation.
- The text goes to the AI provider you chose for tasks, with the names of your lists and their
  other names, and the current date, time and time zone.
- The task goes to your Google account: Google Tasks, and Google Calendar for tasks with a time.
  Mirza reads the names of your task lists, and the tasks just changed in a list when it has
  to check whether a task arrived. It changes only what it added.
- On your computer, Mirza keeps the waiting tasks and the last 20 saved ones (what you said and
  the task) in its data folder, and the sign-in with your API keys. Signing out removes the
  sign-in, the saved list of recent tasks and the copy of your list names.
- Mirza's log never has what you said or the sign-in, unless you run it with
  `MIRZA_LOG=mirza=debug`.

All of it goes through the proxy in **Settings > General**, like everything else Mirza sends.

## Troubleshooting

- **"Access blocked: Mirza has not completed the Google verification process"**: the app is
  still in Testing. Publish it ([step 5](#5-publish-the-app)).
- **Signed out every week**: the same; publish the app, then sign in again.
- **"The Tasks API (or Calendar API) is turned off in your Google Cloud project"**: turn it on
  ([step 2](#2-turn-on-the-tasks-and-calendar-apis)), then click **Try now**.
- **Tasks with a time get no event**: you didn't tick the Calendar box at sign-in. Sign out, sign
  in again and tick both boxes.
- **"This is a Web application client"**: create a **Desktop app** client instead
  ([step 6](#6-create-the-client)).
- **"Couldn't reach Google"**: Google may be blocked where you are. Set a proxy in **Settings >
  General**.
- **Tasks land in the wrong list**: give the right list other names under **Lists**.
- **No Undo or Edit buttons on Linux**: your desktop's notifications don't show buttons. Use
  the tray menu or the settings window.
