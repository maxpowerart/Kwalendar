// In-browser stand-in for the Rust core, so the UI can be previewed with `npm run dev` outside Tauri.
import type { Account, Calendar, EditTarget, EventInput, Instance, Snapshot } from './api';
import { addDays, isoDate, startOfDay, startOfWeek } from './dates';

const listeners = new Map<string, Set<(p: unknown) => void>>();
const emit = (event: string, payload: unknown = null) => listeners.get(event)?.forEach((cb) => cb(payload));

const accounts: Account[] = [
  { id: 'a1', email: 'me@gmail.com', name: 'Личный', color: '#A57CF0', status: 'ok', error: null, lastSync: Date.now() },
  { id: 'a2', email: 'me@company.com', name: 'Работа', color: '#5B8DEF', status: 'ok', error: null, lastSync: Date.now() },
  { id: 'a3', email: 'family@gmail.com', name: 'Семья', color: '#E5604F', status: 'reauth', error: 'Доступ отозван', lastSync: Date.now() - 7200_000 },
];

const cal = (accountId: string, id: string, summary: string, color: string, primary = false, role = 'owner', selected = true): Calendar => ({
  accountId, id, summary, color, selected, accessRole: role, primary, timeZone: 'Europe/Moscow', defaultReminders: [10],
});

const calendars: Calendar[] = [
  cal('a1', 'personal', 'Личное', '#C37BE8', true),
  cal('a1', 'sport', 'Спорт', '#3DC28B'),
  cal('a1', 'bdays', 'Дни рождения', '#E85D8A', false, 'reader'),
  cal('a2', 'work', 'Основной', '#3D8BD8', true),
  cal('a2', 'team', 'Команда', '#2EC4D6', false, 'writer'),
  cal('a2', 'releases', 'Релизы', '#E8C13D', false, 'reader'),
  cal('a3', 'family', 'Общий', '#E8665A', true),
  cal('a3', 'holidays', 'Праздники России', '#858792', false, 'reader', false),
];

interface Tpl {
  id: string;
  cal: string;
  title: string;
  /** weekday 0=Mon..6=Sun, or 'weekdays' */
  days: number[];
  start?: string;
  end?: string;
  allDaySpan?: number;
  meet?: boolean;
  location?: string;
  attendees?: number;
  weekOffset?: number;
}

const W = [0, 1, 2, 3, 4];
let templates: Tpl[] = [
  { id: 'standup', cal: 'work', title: 'Стендап', days: W, start: '09:30', end: '10:00', meet: true, attendees: 6 },
  { id: 'plan', cal: 'work', title: 'Планирование спринта', days: [0], start: '11:00', end: '12:30', attendees: 8 },
  { id: 'gym', cal: 'sport', title: 'Зал', days: [0, 3], start: '19:00', end: '20:00' },
  { id: 'oneone', cal: 'work', title: '1:1 с Ильёй', days: [1], start: '14:00', end: '15:00', meet: true, attendees: 2 },
  { id: 'focus', cal: 'team', title: 'Фокус-время', days: [1, 3], start: '16:00', end: '17:30' },
  { id: 'lunch', cal: 'personal', title: 'Обед с Катей', days: [2], start: '12:00', end: '13:00', location: 'Кафе «Пушкин»' },
  { id: 'interview', cal: 'work', title: 'Интервью: Frontend', days: [2], start: '15:00', end: '16:00', meet: true, attendees: 3 },
  { id: 'review', cal: 'team', title: 'Код-ревью', days: [3], start: '10:00', end: '11:00' },
  { id: 'mkt', cal: 'work', title: 'Синк с маркетингом', days: [3], start: '13:00', end: '14:00', attendees: 4 },
  { id: 'vendor', cal: 'personal', title: 'Звонок подрядчику', days: [3], start: '13:30', end: '14:30' },
  { id: 'design', cal: 'work', title: 'Дизайн-ревью Kwalendar', days: [3], start: '15:00', end: '16:30', meet: true, attendees: 5 },
  { id: 'retro', cal: 'team', title: 'Ретро', days: [3], start: '17:30', end: '18:30' },
  { id: 'demo', cal: 'work', title: 'Демо спринта', days: [4], start: '11:00', end: '12:00', location: 'Переговорная «Байкал»' },
  { id: 'cinema', cal: 'personal', title: 'Кино', days: [4], start: '18:30', end: '20:00' },
  { id: 'run', cal: 'sport', title: 'Пробежка', days: [5], start: '10:00', end: '11:30', location: 'Парк Горького' },
  { id: 'family', cal: 'family', title: 'Семейный обед', days: [6], start: '13:00', end: '15:00' },
  { id: 'freeze', cal: 'releases', title: 'Заморозка релиза 2.4', days: [2], allDaySpan: 2, weekOffset: 0 },
  { id: 'bday', cal: 'bdays', title: 'ДР Ани', days: [5], allDaySpan: 1, weekOffset: 0 },
];
const created: Instance[] = [];
const removed = new Set<string>();
const overrides = new Map<string, Partial<Instance>>();

function at(day: Date, hm: string): number {
  const [h, m] = hm.split(':').map(Number);
  const d = new Date(day);
  d.setHours(h, m, 0, 0);
  return d.getTime();
}

function make(t: Tpl, day: Date): Instance {
  const c = calendars.find((x) => x.id === t.cal)!;
  const allDay = !!t.allDaySpan;
  const start = allDay ? startOfDay(day).getTime() : at(day, t.start!);
  const end = allDay ? addDays(startOfDay(day), t.allDaySpan!).getTime() : at(day, t.end!);
  const names = ['Илья Петров', 'Катя Смирнова', 'Петя Орлов', 'Оля Иванова', 'Дима Ким', 'Аня Лис', 'Саша Ли'];
  const eventId = `${t.id}_${isoDate(day).replaceAll('-', '')}`;
  return {
    key: `${c.accountId}|${c.id}|${eventId}`,
    accountId: c.accountId,
    calendarId: c.id,
    eventId,
    masterId: t.allDaySpan ? null : t.id,
    title: t.title,
    start,
    end,
    allDay,
    startDate: allDay ? isoDate(start) : null,
    endDate: allDay ? isoDate(end) : null,
    location: t.location ?? null,
    description: t.meet ? 'Повестка в документе. Ссылка: https://meet.google.com/abc-defg-hij' : null,
    meetLink: t.meet ? 'https://meet.google.com/abc-defg-hij' : null,
    htmlLink: 'https://calendar.google.com/',
    color: c.color,
    attendees: Array.from({ length: t.attendees ?? 0 }, (_, i) =>
      i === 1
        ? { email: 'me@company.com', name: null, response: 'accepted' as const, isSelf: true, organizer: false, optional: false }
        : { email: `p${i}@company.com`, name: names[i % names.length], response: (['accepted', 'tentative', 'declined', 'needsAction'] as const)[i % 4], isSelf: false, organizer: i === 0, optional: false },
    ),
    organizer: t.attendees ? names[0] : null,
    organizerSelf: !t.attendees,
    selfResponse: t.attendees && t.attendees > 1 ? 'accepted' : null,
    recurring: !t.allDaySpan,
    canEdit: c.accessRole !== 'reader',
    pending: false,
    reminders: [10],
    remindersDefault: true,
  };
}

function instances(from: number, to: number): Instance[] {
  const out: Instance[] = [];
  const thisWeek = startOfWeek(new Date()).getTime();
  for (let d = startOfDay(from); d.getTime() < to; d = addDays(d, 1)) {
    const wd = (d.getDay() + 6) % 7;
    const weekStart = startOfWeek(d).getTime();
    for (const t of templates) {
      if (!t.days.includes(wd)) continue;
      if (t.weekOffset !== undefined && weekStart !== thisWeek + t.weekOffset * 7 * 86400_000) continue;
      const c = calendars.find((x) => x.id === t.cal)!;
      if (!c.selected) continue;
      const inst = make(t, d);
      if (removed.has(inst.key)) continue;
      out.push({ ...inst, ...overrides.get(inst.key) });
    }
  }
  for (const i of created) if (i.end > from && i.start < to && !removed.has(i.key)) out.push(i);
  return out.sort((a, b) => a.start - b.start);
}

function changed() {
  setTimeout(() => emit('kw:changed'), 10);
}

function inputToInstance(input: EventInput, base?: Instance): Instance {
  const c = calendars.find((x) => x.accountId === input.accountId && x.id === input.calendarId)!;
  const id = base?.eventId ?? Math.random().toString(16).slice(2);
  const start = input.allDay ? startOfDay(new Date(input.startDate! + 'T00:00')).getTime() : input.start!;
  const end = input.allDay ? startOfDay(new Date(input.endDate! + 'T00:00')).getTime() : input.end!;
  return {
    ...(base ?? make({ id, cal: c.id, title: input.title, days: [], start: '00:00', end: '01:00' }, new Date())),
    key: base?.key ?? `${c.accountId}|${c.id}|${id}`,
    eventId: id,
    title: input.title || '(без названия)',
    start,
    end,
    allDay: input.allDay,
    startDate: input.allDay ? input.startDate! : null,
    endDate: input.allDay ? input.endDate! : null,
    location: input.location ?? null,
    description: input.description ?? null,
    color: c.color,
    reminders: input.reminders ?? c.defaultReminders,
    remindersDefault: input.reminders == null,
    meetLink: input.addMeet ? 'https://meet.google.com/new' : base?.meetLink ?? null,
    pending: false,
  };
}

export async function mockInvoke(cmd: string, args: Record<string, any> = {}): Promise<unknown> {
  switch (cmd) {
    case 'snapshot': {
      const s: Snapshot = {
        accounts,
        calendars,
        config: {
          googleClientId: 'demo.apps.googleusercontent.com',
          googleClientSecret: '••••',
          remindersEnabled: true,
          notifyChanges: true,
          pollFocusedSecs: 30,
          pollBackgroundSecs: 180,
          view: 'week',
          sidebar: true,
          dayStartHour: 8,
          theme: 'system',
          relayUrl: '',
          relayKey: '',
          checkUpdates: true,
          skippedVersion: '',
        },
        sync: { state: 'idle', lastSync: Date.now() - 12_000, pending: 0, message: null },
        mutedUntil: 0,
        pushActive: false,
        builtinClient: true,
      };
      return s;
    }
    case 'instances':
      return instances(args.from, args.to);
    case 'search': {
      const q = String(args.q).toLowerCase();
      const now = Date.now();
      const list = instances(now - 14 * 86400_000, now + 30 * 86400_000).filter((i) => i.title.toLowerCase().includes(q));
      const seen = new Set<string>();
      return list.filter((i) => (seen.has(i.title) ? false : (seen.add(i.title), true))).slice(0, 8);
    }
    case 'set_calendar_selected': {
      const c = calendars.find((x) => x.accountId === args.accountId && x.id === args.calendarId);
      if (c) c.selected = args.selected;
      changed();
      return null;
    }
    case 'create_event': {
      const inst = inputToInstance(args.input);
      created.push(inst);
      changed();
      return inst.key;
    }
    case 'update_series':
    case 'update_event': {
      const t: EditTarget = args.target;
      const key = `${t.accountId}|${t.calendarId}|${t.eventId}`;
      const own = created.find((i) => i.key === key);
      if (own) Object.assign(own, inputToInstance(args.input, own));
      else {
        const base = instances(t.start ?? Date.now() - 86400_000 * 40, (t.start ?? Date.now()) + 86400_000 * 40).find((i) => i.key === key);
        if (base) overrides.set(key, inputToInstance(args.input, base));
      }
      changed();
      return null;
    }
    case 'move_event': {
      const t: EditTarget = args.target;
      const key = `${t.accountId}|${t.calendarId}|${t.eventId}`;
      const own = created.find((i) => i.key === key);
      if (own) Object.assign(own, { start: args.start, end: args.end });
      else overrides.set(key, { ...overrides.get(key), start: args.start, end: args.end });
      changed();
      return null;
    }
    case 'delete_event': {
      const t: EditTarget = args.target;
      if (args.scope === 'series' && t.masterId) templates = templates.filter((x) => x.id !== t.masterId);
      else removed.add(`${t.accountId}|${t.calendarId}|${t.eventId}`);
      changed();
      return null;
    }
    case 'respond': {
      const t: EditTarget = args.target;
      const key = `${t.accountId}|${t.calendarId}|${t.eventId}`;
      overrides.set(key, { ...overrides.get(key), selfResponse: args.response });
      changed();
      return null;
    }
    case 'add_account':
      throw new Error('В браузерном превью вход в Google недоступен — запустите приложение через `npm run tauri dev`');
    case 'update_info':
    case 'update_check':
      return {
        current: '0.1.0',
        available: { version: '0.2.0', notes: 'Новое\n- Светлая тема и фирменный стиль\n- Перетаскивание событий между днями месяца\n\nИсправлено\n- Напоминания после сна иногда приходили дважды', url: 'Kwalendar_0.2.0_x64-setup.exe', sha256: '0'.repeat(64), published: '2026-11-02', manifestUrl: 'https://github.com/maxpowerart/Kwalendar/releases/download/v0.2.0/update.json' },
        withdrawn: null,
        checkedAt: Date.now(),
        error: null,
      };
    case 'update_list':
      return [
        { version: '0.2.0', published: '2026-11-02', prerelease: false, withdrawn: false, reason: '', manifestUrl: 'm2' },
        { version: '0.1.1', published: '2026-10-28', prerelease: false, withdrawn: true, reason: 'ломает синхронизацию повторов', manifestUrl: 'm11' },
        { version: '0.1.0', published: '2026-10-22', prerelease: false, withdrawn: false, reason: '', manifestUrl: 'm1' },
      ];
    case 'update_manifest':
      return { version: args.url === 'm1' ? '0.1.0' : args.url === 'm11' ? '0.1.1' : '0.2.0', notes: 'Описание версии', url: 'x.exe', sha256: '0'.repeat(64), published: null, manifestUrl: args.url };
    case 'update_install':
      throw new Error('В браузерном превью установка недоступна');
    case 'autostart_get':
      return true;
    default:
      return null;
  }
}

export async function mockListen(event: string, cb: (p: unknown) => void): Promise<() => void> {
  if (!listeners.has(event)) listeners.set(event, new Set());
  listeners.get(event)!.add(cb);
  return () => listeners.get(event)?.delete(cb);
}
