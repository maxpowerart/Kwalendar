# Changelog

Each release's section is shown in Kwalendar's "Update available" dialog and on its GitHub
release page. The heading must be `## <version>` (the release workflow looks it up by version).

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
