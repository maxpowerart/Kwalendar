# Kwalendar push relay

Optional. Without it Kwalendar polls Google (every 30 s with the window active, every 3 min in the tray). With it, Google reports changes itself through `events.watch`: changes arrive within seconds, and polling becomes rare (2 min / 15 min), only as a safety net.

The relay stores only a counter per channel key. No event data passes through it: Google sends just a "something changed in the calendar" signal, and the app fetches the changes directly from Google.

## Deploying (Cloudflare Workers, free plan)

```bash
cd relay
npx wrangler login
npx wrangler deploy
```

Wrangler prints an address like `https://kwalendar-relay.<name>.workers.dev`. Paste it into Kwalendar: Settings → Sync → Push relay. The app generates a secret channel key and subscribes every calendar to notifications. Subscriptions renew themselves; Google keeps them for up to 7 days.

## Protocol

- `POST /google/<key>` — Google posts here. A request is accepted only with the `X-Goog-Channel-Token: <key>` header.
- `GET /wait/<key>?since=N` — the app's long poll. Answers `{"seq": M}` as soon as `M ≠ N`, otherwise after about 50 s.
