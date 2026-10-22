# Kwalendar

Быстрый десктопный календарь для нескольких аккаунтов Google. Windows сейчас, macOS/Linux — тот же код.

- **Local-first**: окно открывается сразу с данными из локальной SQLite, сеть работает в фоне.
- **Экономная синхронизация**: только изменения по `syncToken`; 30 с при активном окне, 3 мин в трее, сразу после выхода из сна и при возврате в окно.
- **Офлайн**: правки применяются мгновенно и уходят в Google из очереди, когда появится сеть.
- **Уведомления Windows** с кнопками «Присоединиться» (Meet/Zoom/Teams) и «Через 5 мин»; сообщения о переносах и новых приглашениях.
- **Несколько аккаунтов Google**, refresh-токены в Диспетчере учётных данных Windows.
- **Ctrl+K** — быстрый ввод фразой («Созвон с Петей завтра в 15 на 30 мин #работа») и поиск; **Ctrl+Alt+K** — из любого окна.
- Неделя / день / месяц / повестка, перетаскивание и растягивание событий, создание выделением.

Стек и архитектура: [docs/STACK.md](docs/STACK.md).

## Первый запуск

Kwalendar ходит в Google напрямую, поэтому нужен свой OAuth-клиент (бесплатно, один раз):

1. [Google Cloud Console](https://console.cloud.google.com/) → новый проект → включить **Google Calendar API**.
2. **OAuth consent screen** → External, scope `https://www.googleapis.com/auth/calendar`.
   Переведите статус в **In production**: в режиме Testing Google отзывает вход через 7 дней.
3. **Credentials → Create credentials → OAuth client ID → Desktop app**.
4. Вставьте Client ID и Client secret в Kwalendar (экран приветствия или Настройки → Подключение Google) и добавьте аккаунты.

Для разработки можно задать клиент переменными окружения `KW_GOOGLE_CLIENT_ID` / `KW_GOOGLE_CLIENT_SECRET` (при сборке — зашиваются по умолчанию).

## Разработка

Нужны Rust (stable, MSVC), Node 20+, WebView2 (есть в Windows 11).

```bash
npm install
npm run tauri dev
```

UI без Rust, на демо-данных в браузере: `npm run dev` → http://localhost:1420.

Тесты ядра:

```bash
cargo test -p kw-core
```

Сборка:

```bash
npm run tauri build
```

## Устройство

```
crates/kw-core/   ядро без зависимостей от Tauri
  db.rs           SQLite (WAL): аккаунты, календари, события, outbox
  google.rs       клиент Calendar API v3, разбор событий
  auth.rs         OAuth loopback + PKCE, keyring
  sync.rs         инкрементальная синхронизация, отправка outbox, 410/412/429
  recurrence.rs   разворачивание повторов (RRULE/EXDATE) и исключений
  writes.rs       оптимистичные правки: создание, изменение, перенос, удаление, RSVP
  reminders.rs    планировщик напоминаний, отложенные, догоняющие после сна
src-tauri/        оболочка: команды, трей, тосты, автозапуск, горячая клавиша
ui/               SolidJS + TypeScript
```

Данные: `%APPDATA%\com.kwalendar.app\` (`kwalendar.db`, `config.json`).

## Что дальше

- Push-уведомления от Google через небольшой relay (`events.watch`) — изменения мгновенно, опрос реже.
- Batch-запросы Calendar API для аккаунтов с десятками календарей (сейчас — до 4 параллельных запросов).
- Перенос всей серии повторяющегося события (сейчас правка повтора меняет только это вхождение).
- Светлая тема, установщик с подписью и автообновлением.
