# Kwalendar — UI style

Mockups: https://claude.ai/artifact/RNFunnE9x8uzjL9BBQxpZ8. Logo: `design/logo/`.

The style comes from GitKwaken (the "Kwak" style, `GitKwaken/assets/styles/kwak.toml`), carried over to a calendar.

## Rules

1. **Navy only in the window chrome.** The title bar with the logo, search and window buttons is `#0F2240` in the dark theme and mint `#DDF0EE` in the light one. The rest of the interface is neutral grey: no blue tint and no brand fills.
2. **Solid teal only for what is selected and active.** That is the active view in the switcher, the selected settings section, the "Yes" answer, the main button of a dialog. Highlighted rows are teal at 22–24 % opacity.
3. **Orange means "today" and "now"**, like the orange cell in the logo. It marks today's date (in the mini month, the week header and the month grid), the current-time line, "in N min" and warnings (offline, "Sign in again"). Orange is used nowhere else.
4. **Icons are outlined, teal, 2px stroke, round caps.** Some icons carry an orange "node" dot, like the commit nodes in GitKwaken: sync, settings, "Create".
5. **Dense layout.** Segoe UI 13 px, list rows 24–30 px, radii 3–4 px (6 px for dialogs). Sidebar section headers are all caps with a disclosure arrow, as in GitKwaken.
6. **Events are a translucent fill in the calendar's colour with light text** (dark text in the light theme). The selected event is filled solid, with navy text and an outline. Past events are at 50 % opacity.
7. **Fredoka is used only for the "Kwalendar" wordmark** in the title bar. The "K" is aqua.

## Tokens

| | dark | light |
|---|---|---|
| chrome | `#0F2240` | `#DDF0EE` |
| chrome-text | `#E8EDF6` | `#0F2240` |
| panel (sidebar, event panel) | `#25272B` | `#F5F5F5` |
| window (grid) | `#2B2D31` | `#FFFFFF` |
| extreme (input fields) | `#1E1F22` | `#FFFFFF` |
| line | `#3A3C42` | `#E0E0E0` |
| text / muted | `#DCDDDF` / `#979797` | `#1F1F1F` / `#6B6B6B` |
| accent (selected) | `#1F988D` | `#158C82` |
| icon | `#3CBAAF` | `#158C82` |
| node (today, now) | `#FD9921` | `#E07C00` |
| selection | `rgba(60,186,175,.22)` | `rgba(21,140,130,.16)` |
| button / border | `#33353A` / `#43454B` | `#FFFFFF` / `#BEBEBE` |

Default calendar colours (the Kwak graph lanes): `#3D8BD8` `#2EC4D6` `#E8C13D` `#C37BE8` `#3DC28B` `#E85D8A` `#E8665A`.

## Brand

Navy `#0F2240`, Aqua `#2EC4B6`, Orange `#FF9F1C`, Mint `#E6F5F4`. The calendar cells in the logo are `#D3EFEC`: in pure mint they would not show on white. Tagline: PLAN BRIGHTER DAYS.

| File | What it is |
|---|---|
| `logo/kwalendar-mark.svg` | the mark without text (for assets, no `<style>`) |
| `logo/kwalendar-icon.svg` | the app icon on a navy square; `src-tauri/icons` can be generated from it with `npx tauri icon` |
| `logo/kwalendar-mono.svg` | navy monochrome |
| `logo/kwalendar-logo.svg` | mark, wordmark and tagline (the Fredoka font is loaded from Google Fonts) |
| `logo/kwalendar-lockup.svg`, `logo/kwalendar-lockup-dark.svg` | horizontal lockup for light backgrounds and for navy |
