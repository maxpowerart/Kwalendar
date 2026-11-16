# Kwalendar — stack and architecture

A desktop client for Google Calendar. Priorities: speed, a pleasant UI, frugal and reliable sync, notifications, several Google accounts. Windows first, then macOS and Linux (mobile later).

## Target metrics

| Metric | Target |
|---|---|
| Cold start to the first frame with data | < 400 ms (data from the local DB, no network) |
| Switching week/month | < 16 ms (1 frame) |
| RAM in the tray | < 80 MB |
| Installer | < 15 MB |
| Delay before someone else's change shows up | ≤ 30 s with the window open |

## Stack

| Layer | Choice | Why |
|---|---|---|
| Shell | **Tauri 2** (Rust + WebView2) | ~10 MB binary, little RAM, native tray/notifications/autostart/hotkeys; macOS and Linux out of the box, mobile targets exist |
| Core | **Rust**, a separate `kw-core` crate with no Tauri dependency | Sync, DB, recurrence, reminders. Reusable on any platform (including through UniFFI for a native mobile UI) |
| UI | **SolidJS + TypeScript + Vite** | Fine-grained reactivity without a VDOM: a grid of hundreds of blocks re-renders only where something changed. ~7 KB runtime |
| Styles | CSS variables (tokens) + CSS modules | Themes (dark/light/accent) with no runtime |
| IPC | Tauri commands + events, types via **tauri-specta** | TS bindings are generated from Rust, so they never drift |
| Storage | **SQLite** (`rusqlite`, bundled) in WAL mode, **FTS5** for search | Local-first: the UI always reads the local DB, the network is background-only |
| Network | `reqwest` (rustls, HTTP/2, gzip) + `tokio` | Keep-alive, compression, parallel requests |
| Dates | `chrono` + `chrono-tz`, recurrence via `rrule` | Recurring events are expanded locally instead of pulling thousands of instances from the server |
| OAuth | `oauth2` crate, PKCE, loopback redirect `127.0.0.1:<random port>` | The flow Google recommends for desktop apps |
| Secrets | `keyring` → Windows Credential Manager | Refresh tokens are never stored in files |
| Notifications | WinRT toasts via the `windows` crate ("Join", "Snooze" buttons) | `tauri-plugin-notification` cannot do toast actions |
| System | `tauri-plugin-autostart`, `-single-instance`, `-global-shortcut`, `-updater`, Tauri tray | Start with Windows into the tray, `Ctrl+Alt+K` for quick add from any window |
| Quick add | `chrono-node` (has a Russian locale) + our own layer for `#calendar`, guests, reminders | A ready-made EN/RU date parser |
| Build | Tauri bundler → MSI/NSIS, signing, auto-update | The installer registers the AppUserModelID; Windows toasts do not work without it |

### Why not something else

- **Electron** — a 150+ MB installer and 250+ MB of RAM. Contradicts the main selling point.
- **WinUI 3 / WPF** — Windows only; going cross-platform would mean a rewrite.
- **Flutter** — good-looking, but weaker Windows integration (toasts with actions, tray, AUMID) and worse text rendering on desktop.
- **Avalonia** — a solid alternative for a C# team. But a polished custom UI is faster to build on the web, and a Rust core is easier to reuse.
- **Pure Rust UI (egui/iced/Slint)** — fast, but a "pleasant interface" would take several times more work.

## Sync

Principle: **local-first**. The UI never waits for the network. Every change is written to SQLite first, then a background worker syncs.

**Reading (Google → local)**

1. Per account: `CalendarList.list` with a `syncToken`. Per calendar: `Events.list` with a `syncToken`, `showDeleted=true`, `singleEvents=false` (we store recurrence masters and exceptions and expand them ourselves).
2. First sign-in in two stages: first a quick request with `timeMin/timeMax` for ±4 weeks so the screen fills in seconds, then a full paged download in the background (`maxResults=2500`) that yields the `nextSyncToken`.
3. Incremental checks fetch only deltas, usually an empty answer of a few hundred bytes:
   - every 30 s while the window is active; 2–5 min in the tray;
   - right away after wake-up from sleep, network recovery, window focus, and our own writes.
4. Many calendars → one **batch** request (up to 50 sub-requests), `fields=` for partial responses, gzip.
5. `410 Gone` → full resync of that calendar only. `403 rateLimitExceeded`/`429` → exponential backoff with jitter.
6. Phase 2 (optional): push through `events.watch`. It needs a public HTTPS webhook, hence a small relay (e.g. a Cloudflare Worker) that wakes the client up. Changes then arrive instantly, and polling drops to a rare mode (every 10–15 min).

**Writing (local → Google)**

- Optimistic apply to the DB + an `outbox` table. The worker sends with `If-Match: <etag>`.
- `412 Precondition Failed` → re-read the event; simple fields are merged, on a real conflict the server wins and the user gets a "changed by another participant" notice.
- Offline: the queue builds up and is sent when the network is back. The UI shows "N changes queued".

**DB schema (core)**: `accounts`, `calendars` (sync_token, color, selected), `events` (raw JSON + indexed fields, etag, recurrence), `instances` (expanded occurrences for a ±1–2 year window, recomputed when the master changes), `reminders` (fire_at, indexed), `outbox`, `events_fts`.

## Notifications

- Scheduler in the core: a single `tokio` timer until the nearest `reminders.fire_at`. Recomputed after sync, a time-zone change and wake-up from sleep (`WM_POWERBROADCAST`).
- If a reminder was missed while the PC slept: less than 15 min late — show it, otherwise mark it as missed in the tray.
- Toast: "Join" (the Meet/Zoom link from the event), "Snooze 5/10/15 min", "Dismiss".
- Notices about changes by others (moved, cancelled, new invitation) come from sync deltas and can be turned off.
- "Do not disturb for 1 h" from the tray.

## Several Google accounts

- Each account has its own refresh token in the keyring (`kwalendar/<google sub>`) and its own sync worker. An error in one account does not affect the others (the UI shows "Sign in again" and keeps the cache).
- Scopes: `openid email profile https://www.googleapis.com/auth/calendar`.
- **Important about Google OAuth:** the `calendar` scope is *sensitive*.
  - While the app is in **Testing** status, refresh tokens live for **7 days**: you would have to sign in again every week. For personal use, move the app to **In production** without verification: there will be an "unverified app" warning screen and a 100-user limit, but tokens will not expire.
  - A public release needs Google's app verification (enough for a sensitive scope; a CASA audit is only needed for restricted ones).
  - The `client_secret` of a desktop OAuth client is not a secret according to Google's documentation; protection comes from PKCE + loopback.

## How to be fast

- The window is shown only after the first frame with data from the DB. The WebView2 background matches the theme, so there is no white flash.
- The core keeps expanded occurrences for ±6 weeks around the current one in memory; neighbouring ranges are prefetched.
- Overlapping events are laid out in TS on the fly (O(n log n) per day), the DOM holds only the visible weeks; the agenda is virtualised (`@tanstack/solid-virtual`).
- No heavy UI libraries. Icons are inline SVG, the Geist font is bundled locally.
- Everything works from the keyboard: `Ctrl+K` — search/quick add, `1–4` — views, `T` — today, `J/K` — next/previous, `N` — new event.

## Repository layout

```
kwalendar/
  crates/kw-core/     # sync, db, google api, recurrence, reminders — no Tauri
  src-tauri/          # shell: commands, tray, toasts, autostart, updates
  ui/                 # SolidJS + TS
  docs/
```

## Cross-platform

- **macOS** — the same Tauri; notifications through UserNotifications, tray → menu bar.
- **Linux** — Tauri on WebKitGTK (slower than WebView2, needs separate profiling).
- **Mobile** — Tauri mobile or a native UI on top of `kw-core` through UniFFI. The core has been isolated for this from day one.

## Milestones

1. **M1** — one account, read-only, week view, local DB, incremental sync.
2. **M2** — several accounts, offline, calendar selection, colours.
3. **M3** — reminders, toasts with actions, tray, autostart.
4. **M4** — create and edit, outbox, conflicts, `Ctrl+K` quick add.
5. **M5** — month and agenda, FTS search, settings, installer and auto-update.
6. **M6** (optional) — push relay, macOS build.

## Mockups

Canvas: https://claude.ai/artifact/RNFunnE9x8uzjL9BBQxpZ8 — week (the main screen), month with the sidebar collapsed, quick add, tray and toasts, account and sync settings.

Visual system of the mockups: dark graphite theme (`#0F1013` background, `#15161A` panels), accent `#8EA2FF`, Geist / Geist Mono fonts, calendar colours as a 20–24 % fill + light text, past events dimmed.
