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

The first lets Mirza add, change and delete tasks. The second lets it add, change and delete
events in calendars you own, and nothing else in your calendar: it can't read your other
calendars or change their settings.

### 5. Publish the app

1. Open [Audience](https://console.cloud.google.com/auth/audience).
2. Under **Publishing status** it says **Testing**. Click **Publish app**, then **Confirm**.
3. It now says **In production**. If Google mentions verification, ignore it: don't click
   **Prepare for verification**.

Why: in Testing, Google signs Mirza out every 7 days. In production without verification, the
sign-in lasts, and up to 100 Google accounts can ever use your client, which is plenty for you
and a few friends. The price is a warning screen at sign-in (see below).

### 6. Create the client

1. Open [Clients](https://console.cloud.google.com/auth/clients) and click **Create client**.
2. **Application type**: **Desktop app**. **Name**: `Mirza desktop`. Click **Create**.
3. A window shows the client ID and the client secret. Click **Download JSON** now: Google
   shows the secret only this once. Keep the file somewhere private. (If you lose it, open the
   client, add a new secret and download that.)

Mirza needs both. For a desktop app the "secret" isn't really secret (Google says so itself:
anyone can pull it out of the program), but there's no reason to post it.

### Signing in

1. Open Mirza's settings, go to **Voice tasks**, and under **Google client** paste the whole
   downloaded JSON file (or the client ID and the client secret separately).
2. Click **Sign in with Google**. Your browser opens Google's sign-in page.
3. Google says **Google hasn't verified this app**. Click **Advanced**, then **Go to Mirza
   (unsafe)**. This is your own app, so that's expected.
4. Google lists what Mirza asks for. **Tick both boxes** (Tasks and Calendar) and click
   **Continue**. Without the calendar box, tasks with a time are saved as tasks only.
5. The browser says you can close the page. The settings window shows your lists.

From a terminal, `mirza google-login` does the same. On a computer without a browser it prints
the address to open on another device; after signing in there, copy the address the browser
ends up at (it starts with `http://127.0.0.1` and shows an error, which is fine) and paste it
into the terminal.

Mirza keeps the sign-in (a "refresh token") in the same file as your API keys, readable only by
you. **Sign out** in the settings removes it and tells Google to cancel it. You can also remove
Mirza's access at [myaccount.google.com/connections](https://myaccount.google.com/connections).

If you don't use voice tasks for six months, Google ends the sign-in by itself; sign in again.
