# Installing Mirza on Android

Mirza for Android is not in the Play Store. Each
[release](https://github.com/erfnemati/mirza/releases/latest) has the app as
`Mirza-<version>-android.apk`. It needs Android 10 or newer and an API key for Soniox (or
another speech service), like the desktop app.

## Install

1. On the phone, open the release page and download the `.apk` file.
2. Open the downloaded file. Android asks whether your browser (or file manager) may install
   apps: allow it, go back, and open the file again.
3. **Play Protect** may stop the install, because the app doesn't come from the Play Store.
   Choose **More details**, then **Install anyway**. If it offers to send the app for a scan, you
   can say no; the app works either way.
4. Open **Mirza**. A short list shows what is still missing. Go through it from the top:
   - **Microphone**: allow it. Mirza records only while the bubble is red.
   - **Notifications**: allow them. Mirza shows one only while it listens, with a Stop button.
   - **Typing**: this opens Android's accessibility settings. Open **Installed apps** (or
     **Downloaded apps**), then **Mirza**, and turn it on. Android warns that the app gets full
     control; Mirza uses it to show the bubble and to type into the field you are in.
   - **API key**: enter your Soniox key (or switch to another service first). If Soniox is
     blocked where you are, set a proxy under **General**.

The bubble now shows over every app. Tap into a text field, tap the bubble, speak, and tap it
again.

## "Restricted setting"

On Android 13 and newer, an app installed from a file can't turn on accessibility at first: the
switch is grey and Android says the setting is restricted. To allow it once:

1. Open **App info** for Mirza (long press its icon, then the "i").
2. Tap the menu at the top right (three dots) and choose **Allow restricted settings**. Confirm
   with your PIN or fingerprint.
3. Go back to the accessibility settings and turn Mirza on.

If the menu has no such item, try to turn Mirza on in the accessibility settings once first; the
item appears after Android has refused it.

## Xiaomi, Redmi and POCO

HyperOS and MIUI stop apps in the background, which turns Mirza's typing off. The setup list has
two more steps there:

- **Autostart**: turn it on for Mirza, so the phone may restart typing after it stops it.
- **Battery saver**: in App info, Battery saver, choose **No restrictions**.

Mirza stays out of the recent apps screen on purpose: "clear all" there would stop it. If typing
still turns off now and then, the setup list says so and takes you to the switch.

## Other ways to start

Besides the bubble, **Bubble and gestures** in Mirza can start it by holding volume down or
pressing it twice, and from Android's accessibility button. The button comes from a second, small
service called **Mirza button**: turn on its shortcut in the accessibility settings to get it.

## Voice tasks

To say tasks into Google Tasks and Calendar, open **Voice tasks** in Mirza and sign in with your
Google account, allowing both Tasks and Calendar. This needs Google Play services. Google may warn
first that it hasn't verified the app; continue past the warning.

## Updates

When you open Mirza, it looks for a new version at most once a day (nothing runs in the
background for it) and shows a card that opens the release page. Download the new `.apk` there
and open it; it installs over the old one and keeps your settings and keys. Android can't install
an older version over a newer one.

## Mirza trial

If you had the earlier test app ("Mirza trial"), uninstall it, or at least turn off "Mirza trial
typing" in the accessibility settings; otherwise two bubbles show.
