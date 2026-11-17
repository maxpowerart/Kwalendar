# Changelog

Each release's section is shown in Kwalendar's "Update available" dialog and on its GitHub
release page. The heading must be `## <version>` (the release workflow looks it up by version).

## 0.3.0

New
- Today popup from the tray: a left click on the Kwalendar icon opens a small window with your
  day next to the taskbar. It shows the next meeting with a Join button, all-day events, the
  day's events with the current time and free gaps (click a gap to create an event there).
  Open an event to see details, answer Yes / Maybe / No, edit or delete it. Type in the field
  at the bottom to add an event ("Call Alex 15:30 for 30 min"): Enter adds it, Tab opens the
  full form. ← → switch days; the arrow button opens the day in the main window.
  The popup hides when you click elsewhere; the pin keeps it open. Right click still opens the menu.
- Ctrl+Alt+K and "New event…" in the tray menu now open this popup with the cursor in quick add.
- An orange dot on the tray icon when an event starts within 15 minutes.

## 0.2.2

Fixed
- Events of about 35–45 minutes in the week view showed only their time, without the title:
  short events now show "Title · time" on one line, and the title is never cut off at the top.

## 0.2.1

Fixed
- After installing an update the old version could start again, and a manual restart was needed:
  the app now restarts only once the new version is in place.
- Calendars by iCal link are requested in the app's language, so texts Google adds to events
  (like "Join with Google Meet") no longer come in the language of the country you are in.

## 0.2.0

New
- Calendars by iCal link (Settings › Accounts › Add by iCal link), for work accounts where the
  admin does not allow API access. In Google Calendar: Settings › the calendar › Integrate
  calendar › Secret address in iCal format. Such calendars are read-only: events, series,
  reminders and Meet links are shown, but events cannot be created, moved or answered.
  Refreshed every 2–3 minutes; the link is kept in the Windows Credential Manager.
- English interface (follows the Windows language or is set in Settings › System).
- Readable calendar names and one-line account headers in the sidebar.

## 0.1.0

First release.

- Week, day, month and agenda views; drag to move and resize events, drag on empty space to create.
- Several Google accounts; local database — the window opens instantly and works offline.
- Incremental sync (batched), optional push through the relay.
- Windows reminders with "Join" and "In 5 min" buttons; notices about moved meetings and invitations.
- Quick add with Ctrl+K in English and Russian, from anywhere with Ctrl+Alt+K.
- Updates from GitHub releases: check, skip a version, install any version and roll back.
