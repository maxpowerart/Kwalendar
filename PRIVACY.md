# Kwalendar — Privacy Policy

_Last updated: 2026-10-22_

Kwalendar is a desktop calendar client for Google Calendar that runs entirely on your computer.

## What data Kwalendar accesses

When you connect a Google account, Kwalendar requests these Google permissions:

- `openid`, `email`, `profile` — to show which account is connected;
- `https://www.googleapis.com/auth/calendar` — to read your calendars and events and to create, change, delete and answer events when you ask it to.

## Where the data goes

- **It stays on your computer.** Calendars and events are stored in a local database (`%APPDATA%\com.kwalendar.app\kwalendar.db`) so the app opens instantly and works offline.
- **Sign-in tokens** (OAuth refresh tokens) are stored in the Windows Credential Manager, not in files.
- Kwalendar talks **directly to Google's APIs**. There is no Kwalendar server; the developer receives none of your data.
- **Optional push relay.** If you set one up yourself (see `relay/`), Google notifies that relay that "something changed in a calendar". The relay stores only a counter per channel and never receives event contents.
- **Update checks** download release information from GitHub (`github.com/maxpowerart/Kwalendar`). No personal data is sent.

Kwalendar does not sell, share, or transfer Google user data to anyone, does not use it for advertising, and does not use it to train AI or machine-learning models. Kwalendar's use of information received from Google APIs adheres to the [Google API Services User Data Policy](https://developers.google.com/terms/api-services-user-data-policy), including the Limited Use requirements.

## Deleting your data

- **Settings → Accounts → Disconnect** removes the account's local copy and its token from the Credential Manager.
- Uninstalling Kwalendar removes the app; delete `%APPDATA%\com.kwalendar.app` to remove all local data.
- You can revoke Kwalendar's access at any time at <https://myaccount.google.com/permissions>.

## Contact

Questions: open an issue at <https://github.com/maxpowerart/Kwalendar/issues> or write to thepowerart@gmail.com.

---

**Кратко по-русски.** Kwalendar работает только на вашем компьютере. События хранятся в локальной базе, токены входа лежат в Диспетчере учётных данных Windows. Приложение обращается напрямую к Google, своего сервера у него нет, разработчик ваших данных не получает. Данные Google никому не передаются и не используются для рекламы или обучения моделей. Чтобы удалить данные, отключите аккаунт в настройках или отзовите доступ на myaccount.google.com/permissions.
